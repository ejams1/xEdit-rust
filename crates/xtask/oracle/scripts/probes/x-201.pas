{ parity probe: x-201 }
unit P_x_201;

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
  v := nil; AddMessage(IntToStr(VarType(v)));
  AddMessage('after');
end;
end.
