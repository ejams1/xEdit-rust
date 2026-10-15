{ parity probe: x-268 }
unit P_x_268;

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
  v := FloatToStr(nil); AddMessage(v);
  AddMessage('after');
end;
end.
