{ parity probe: x-231 }
unit P_x_231;

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
  v := FileByIndex(0); AddMessage(IntToStr(VarType(v)));
  AddMessage('after');
end;
end.
