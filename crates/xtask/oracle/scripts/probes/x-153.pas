{ parity probe: x-153 }
unit P_x_153;

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
  v := sl.Count; v := v + '1'; AddMessage(IntToStr(VarType(v)) + ' ' + VarToStr(v));
  AddMessage('after');
end;
end.
