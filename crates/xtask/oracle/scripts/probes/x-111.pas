{ parity probe: x-111 }
unit P_x_111;

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
  v := 1; v := v + '1'; AddMessage(VarToStr(v)); AddMessage(IntToStr(VarType(v)));
  AddMessage('after');
end;
end.
