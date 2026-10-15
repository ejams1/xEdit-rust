{ parity probe: x-168 }
unit P_x_168;

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
  v := Unassigned; v := v + 1; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
