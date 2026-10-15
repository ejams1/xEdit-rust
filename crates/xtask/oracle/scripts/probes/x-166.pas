{ parity probe: x-166 }
unit P_x_166;

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
  v := Unassigned; v := v = ''; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
