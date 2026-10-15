{ parity probe: x-334 }
unit P_x_334;

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
  v := sl.ClassNameIs('TStringList'); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
