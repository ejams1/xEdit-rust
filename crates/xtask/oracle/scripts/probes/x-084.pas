{ parity probe: x-084 }
unit P_x_084;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  AddMessage(FloatToStr(1e20));
  AddMessage('after');
end;
end.
