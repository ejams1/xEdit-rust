{ parity probe: x-164 }
unit P_x_164;

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
  v := Unassigned; v := v = Unassigned; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
