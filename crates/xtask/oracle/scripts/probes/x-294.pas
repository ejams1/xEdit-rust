{ parity probe: x-294 }
unit P_x_294;

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
  sl.AddObject('x', FileByIndex(0)); AddMessage(Name(ObjectToElement(sl.Objects[1])));
  AddMessage('after');
end;
end.
