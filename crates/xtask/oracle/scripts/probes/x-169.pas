{ parity probe: x-169 }
unit P_x_169;

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
  v := Unassigned; v := v + 'x'; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
