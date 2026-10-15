{ parity probe: x-147 }
unit P_x_147;

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
  AddMessage(FloatToStr(StrToFloat('.5')));
  AddMessage('after');
end;
end.
