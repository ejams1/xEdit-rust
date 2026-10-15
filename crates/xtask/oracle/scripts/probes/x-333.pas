{ parity probe: x-333 }
unit P_x_333;

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
  v := sl.InheritsFrom(TStrings); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
